#!/usr/bin/env perl
use strict;
use warnings;

use Mail::Milter::Server;
use Mail::Milter::Context qw(SMFIS_CONTINUE SMFIS_ACCEPT SMFIS_REPLACEBODY);
use MIME::Parser;
use MIME::Entity;
use JSON::MaybeXS;
use Data::UUID;
use File::Slurp qw(write_file);
use File::Path qw(make_path);
use POSIX qw(strftime);
use IO::String;
use Encode qw(decode encode);

# Configuration from environment
my $PIXEL_BASE_URL = $ENV{PIXEL_BASE_URL} // 'https://localhost:8443/pixel?id=';
my $REQUIRE_OPT_IN = $ENV{REQUIRE_OPT_IN} // 1;
my $OPT_IN_HEADER = $ENV{OPT_IN_HEADER} // 'X-Track-Open';
my $DISCLOSURE_HEADER = $ENV{DISCLOSURE_HEADER} // 'X-Tracking-Notice';
my $INJECT_DISCLOSURE = $ENV{INJECT_DISCLOSURE} // 1;
my $LOG_PATH = $ENV{LOG_PATH} // '/data/pixel/milter.log';

###############################################
# Per-message milter filter object
###############################################

{
    package PixelFilter;
    use parent 'Mail::Milter::Filter';

    sub new {
        my ($class) = @_;
        return bless {
            id => undef,
            headers => {},
            raw_headers => "",
            body => "",
            should_track => 0,
            sender => "",
        }, $class;
    }

    sub envfrom {
        my ($self, $ctx, $sender) = @_;
        $self->{id} = _generate_id();
        $self->{sender} = $sender;
        $self->{headers} = {};
        $self->{raw_headers} = "";
        $self->{body} = "";
        $self->{should_track} = 0;
        
        _log("New message from $sender, ID: $self->{id}");
        return SMFIS_CONTINUE;
    }

    sub header {
        my ($self, $ctx, $name, $value) = @_;
        
        # Store headers in both formats
        $self->{raw_headers} .= "$name: $value\n";
        $self->{headers}{lc($name)} = $value;
        
        # Check for opt-in header
        if ($REQUIRE_OPT_IN && lc($name) eq lc($OPT_IN_HEADER)) {
            $self->{should_track} = ($value =~ /^(yes|true|1|on)$/i);
            _log("Opt-in header found: $name: $value, tracking: $self->{should_track}");
        }
        
        return SMFIS_CONTINUE;
    }

    sub eoh {
        my ($self, $ctx) = @_;
        $self->{raw_headers} .= "\n";
        
        # If opt-in is not required, enable tracking by default
        if (!$REQUIRE_OPT_IN) {
            $self->{should_track} = 1;
        }
        
        # Add disclosure header if configured
        if ($INJECT_DISCLOSURE && $self->{should_track}) {
            $ctx->chgheader($DISCLOSURE_HEADER, 1, "This email contains tracking pixels");
        }
        
        return SMFIS_CONTINUE;
    }

    sub body {
        my ($self, $ctx, $chunk) = @_;
        $self->{body} .= $chunk;
        return SMFIS_CONTINUE;
    }

    sub eom {
        my ($self, $ctx) = @_;
        
        eval {
            my $id = $self->{id};
            my $base = "/data/pixel/$id";
            
            make_path($base);
            
            # Save original message data
            write_file("$base/headers.txt", $self->{raw_headers});
            write_file("$base/body.txt", $self->{body});
            
            my $meta = {
                id => $id,
                created => time(),
                created_str => scalar localtime,
                sender => $self->{sender},
                size => length($self->{raw_headers}) + length($self->{body}),
                tracking_enabled => $self->{should_track} ? JSON::MaybeXS::true : JSON::MaybeXS::false,
                opened => JSON::MaybeXS::false,
                open_count => 0,
                first_open => undef,
                last_open => undef,
            };
            
            write_file("$base/meta.json", encode_json($meta));
            
            # Inject pixel if tracking is enabled
            if ($self->{should_track}) {
                my $modified_body = $self->_inject_pixel($self->{body}, $id);
                if ($modified_body ne $self->{body}) {
                    $ctx->replacebody($modified_body);
                    _log("Pixel injected for message $id");
                } else {
                    _log("No HTML content found for pixel injection in message $id");
                }
            } else {
                _log("Tracking disabled for message $id");
            }
            
        };
        
        if ($@) {
            _log("Error processing message $self->{id}: $@");
        }
        
        return SMFIS_ACCEPT;
    }
    
    sub _inject_pixel {
        my ($self, $body, $id) = @_;
        
        # Try to parse as MIME message
        my $parser = MIME::Parser->new();
        $parser->output_to_core(1);
        $parser->tmp_to_core(1);
        
        my $entity;
        eval {
            my $io = IO::String->new($body);
            $entity = $parser->parse($io);
        };
        
        if ($@ || !$entity) {
            # Fallback: treat as plain text and look for HTML
            return $self->_inject_pixel_simple($body, $id);
        }
        
        # Process MIME entity
        $self->_process_mime_entity($entity, $id);
        
        # Return modified body
        return $entity->stringify;
    }
    
    sub _process_mime_entity {
        my ($self, $entity, $id) = @_;
        
        my $content_type = $entity->mime_type || '';
        
        if ($content_type =~ /^text\/html/i) {
            # HTML part - inject pixel
            my $body = $entity->bodyhandle->as_string;
            my $modified = $self->_inject_pixel_html($body, $id);
            
            if ($modified ne $body) {
                my $io = IO::String->new($modified);
                $entity->bodyhandle($io);
            }
        } elsif ($content_type =~ /^multipart\//i) {
            # Multipart - process each part
            for my $part ($entity->parts) {
                $self->_process_mime_entity($part, $id);
            }
        }
    }
    
    sub _inject_pixel_simple {
        my ($self, $body, $id) = @_;
        
        # Simple HTML detection and injection
        if ($body =~ /<html\b/i || $body =~ /<body\b/i) {
            return $self->_inject_pixel_html($body, $id);
        }
        
        return $body;
    }
    
    sub _inject_pixel_html {
        my ($self, $html, $id) = @_;
        
        my $pixel_url = $PIXEL_BASE_URL . $id;
        my $pixel_img = qq{<img src="$pixel_url" width="1" height="1" style="display:none;border:0;outline:0;" alt="" />};
        
        # Try to inject before closing body tag
        if ($html =~ s{(</body\s*>)}{$pixel_img$1}i) {
            return $html;
        }
        
        # Try to inject before closing html tag
        if ($html =~ s{(</html\s*>)}{$pixel_img$1}i) {
            return $html;
        }
        
        # Fallback: append to end
        return $html . $pixel_img;
    }
}

###############################################
# Utilities
###############################################

sub _generate_id {
    my $ug = Data::UUID->new;
    return strftime("%Y%m%d-%H%M%S", localtime) . "-" . $ug->create_str;
}

sub _log {
    my ($message) = @_;
    my $timestamp = strftime("%Y-%m-%d %H:%M:%S", localtime);
    my $log_entry = "[$timestamp] $message\n";
    
    eval {
        if (open my $fh, '>>', $LOG_PATH) {
            print $fh $log_entry;
            close $fh;
        }
    };
    
    # Also log to stderr for debugging
    print STDERR $log_entry;
}

###############################################
# Run the milter server
###############################################

# Ensure log directory exists
eval {
    my $log_dir = $LOG_PATH;
    $log_dir =~ s{/[^/]+$}{};  # Remove filename
    make_path($log_dir) if $log_dir;
};

my $socket = $ENV{PIXEL_MILTER_SOCKET} // "/var/run/pixelmilter/pixel.sock";

# Clean up existing socket
unlink $socket if -e $socket;

# Ensure socket directory exists
my $socket_dir = $socket;
$socket_dir =~ s{/[^/]+$}{};  # Remove filename
make_path($socket_dir) unless -d $socket_dir;

_log("Starting pixel milter server on socket: $socket");
_log("Configuration: PIXEL_BASE_URL=$PIXEL_BASE_URL, REQUIRE_OPT_IN=$REQUIRE_OPT_IN");

my $server = Mail::Milter::Server->new({
    unix => $socket,
});

$server->register('pixel' => PixelFilter->new());

# Set signal handlers for graceful shutdown
$SIG{TERM} = $SIG{INT} = sub {
    _log("Received shutdown signal, cleaning up...");
    unlink $socket if -e $socket;
    exit 0;
};

_log("Pixel milter server started successfully");
$server->main();
