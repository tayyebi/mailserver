#!/usr/bin/env perl
use strict;
use warnings;
use Mail::Milter::Simple qw(register SMFIS_CONTINUE SMFIS_ACCEPT);
use MIME::Parser;
use MIME::Entity;
use POSIX qw(strftime);
use Data::UUID;
use IO::Socket::UNIX;
use JSON::MaybeXS;
use File::Slurp qw(write_file append_file);
use File::Path qw(make_path);

# Configuration from env
my $socket_path     = $ENV{PIXEL_MILTER_SOCKET} // '/var/run/pixelmilter/pixel.sock';
my $pixel_base_url  = $ENV{PIXEL_BASE_URL} // 'https://localhost/pixel?id=';
my $require_opt_in  = defined $ENV{REQUIRE_OPT_IN} ? $ENV{REQUIRE_OPT_IN} : 1;
my $opt_in_header   = $ENV{OPT_IN_HEADER} // 'X-Track-Open';
my $disclosure_hdr  = $ENV{DISCLOSURE_HEADER} // 'X-Tracking-Notice';
my $inject_disclosure = defined $ENV{INJECT_DISCLOSURE} ? $ENV{INJECT_DISCLOSURE} : 1;
my $log_path        = $ENV{LOG_PATH} // '/data/pixel/requests.log';

# runtime buffers
my %ctx = ();   # per-connection context keyed via $self

sub new {
    my ($class) = @_;
    return bless {}, $class;
}

sub header {
    my ($self, $name, $value) = @_;
    $ctx{$self}->{raw_headers} .= "$name: $value\n";
    push @{ $ctx{$self}->{headers} }, [$name, $value];
    return SMFIS_CONTINUE;
}

sub eoh {
    return SMFIS_CONTINUE;
}

sub body {
    my ($self, $chunk) = @_;
    $ctx{$self}->{body} .= $chunk;
    return SMFIS_CONTINUE;
}

sub eom {
    my ($self) = @_;

    my $raw = ($ctx{$self}->{raw_headers} // '') . ($ctx{$self}->{body} // '');
    # Reset context for this $self so next message starts clean
    my $local_ctx = $ctx{$self};
    delete $ctx{$self};

    # opt-in check
    if ($require_opt_in) {
        unless ($raw =~ /^$opt_in_header:\s*yes/im) {
            return SMFIS_ACCEPT;
        }
    }

    # parse MIME message
    my $parser = MIME::Parser->new;
    $parser->decode_bodies(0);
    my $entity;
    eval { $entity = $parser->parse_data($raw); };
    if ($@ || !$entity) {
        return SMFIS_ACCEPT;
    }

    # create uuid and pixel tag
    my $ug = Data::UUID->new;
    my $uuid = $ug->create_str();
    my $pixel_tag = qq{<img src="$pixel_base_url$uuid" width="1" height="1" style="display:none;" alt="" />};

    # inject disclosure header
    if ($inject_disclosure) {
        $entity->head->replace($disclosure_hdr => 'open tracking enabled');
    }

    # process MIME structure
    eval {
        if (_is_multipart($entity)) {
            _process_multipart($entity, $pixel_tag);
        } else {
            _process_singlepart($entity, $pixel_tag);
        }
    };
    if ($@) {
        return SMFIS_ACCEPT;
    }

    # log mapping (non-PII). Ensure directory exists
    eval { make_path((split('/', $log_path))[0]); };
    my $log = {
        uuid    => $uuid,
        ts      => strftime("%Y-%m-%dT%H:%M:%SZ", gmtime),
        message_id => ($entity->head->get('Message-ID') // ''),
        from    => ($entity->head->get('From') // ''),
        subject => ($entity->head->get('Subject') // ''),
    };
    eval { append_file($log_path, encode_json($log) . "\n"); };

    # serialize entity to scalar
    my $out = '';
    open my $fh, '>', \$out or die "open scalar: $!";
    $entity->print($fh);
    close $fh;

    # replace the message with modified content
    # Mail::Milter::Simple provides replacemsg via Mail::Milter API
    if ($self->can('replacemsg')) {
        $self->replacemsg($out);
    } else {
        # best-effort: set body parts where possible
        # This fallback will rely on entity modifications already applied
        $self->replacemsg($out) if $self->can('replacemsg');
    }

    return SMFIS_ACCEPT;
}

sub _is_multipart {
    my ($e) = @_;
    my $ct = $e->head->get('Content-Type') // '';
    return $ct =~ /multipart\//i;
}

sub _process_multipart {
    my ($e, $pixel) = @_;
    my @parts = $e->parts;
    for my $p (@parts) {
        my $ct = ($p->head->get('Content-Type') // '');
        if ($ct =~ m{text/html}i) {
            _inject_pixel_in_html_part($p, $pixel);
        } elsif ($ct =~ m{multipart/}i) {
            _process_multipart($p, $pixel);
        } else {
            # leave other parts untouched
        }
    }
}

sub _process_singlepart {
    my ($e, $pixel) = @_;
    my $ct = ($e->head->get('Content-Type') // '');
    my $body = _get_body($e);

    if ($ct =~ m{text/html}i) {
        $body = _inject_pixel_in_html($body, $pixel);
        _set_body($e, $body, 'text/html; charset=UTF-8');
    } elsif ($ct =~ m{text/plain}i) {
        my $html = "<html><body><pre>" . _html_escape($body) . "</pre>$pixel</body></html>";
        _set_body($e, $html, 'text/html; charset=UTF-8');
    } else {
        # non-text content: leave as-is
    }
}

sub _inject_pixel_in_html_part {
    my ($part, $pixel) = @_;
    my $body = _get_body($part);
    my $new  = _inject_pixel_in_html($body, $pixel);
    _set_body($part, $new, 'text/html; charset=UTF-8');
}

sub _inject_pixel_in_html {
    my ($html, $pixel) = @_;
    if ($html =~ /<\/body>/i) {
        $html =~ s{</body>}{$pixel</body>}i;
    } else {
        $html .= $pixel;
    }
    return $html;
}

sub _get_body {
    my ($e) = @_;
    my $io = $e->open('r');
    local $/;
    my $data = <$io>;
    $io->close;
    return $data // '';
}

sub _set_body {
    my ($e, $content, $content_type) = @_;
    $e->head->replace('Content-Type' => $content_type);
    my $io = $e->open('w');
    print $io $content;
    $io->close;
}

sub _html_escape {
    my ($s) = @_;
    $s =~ s/&/&amp;/g;
    $s =~ s/</&lt;/g;
    $s =~ s/>/&gt;/g;
    return $s;
}

# register callbacks
register(
    {
        header => sub { return header(@_); },
        eoh    => sub { return eoh(@_); },
        body   => sub { return body(@_); },
        eom    => sub { return eom(@_); },
    },
    'PixelMilter'
);

# prepare socket and env for Mail::Milter to bind
$ENV{MILTER_SOCKET} = "unix:$socket_path";

# Ensure socket path removed if exists
if (-e $socket_path) { unlink $socket_path; }

# Run the milter loop
Mail::Milter::Simple->run;