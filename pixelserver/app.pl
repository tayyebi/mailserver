#!/usr/bin/env perl
use strict;
use warnings;
use Mojolicious::Lite;
use JSON::MaybeXS;
use POSIX qw(strftime);
use File::Path qw(make_path);
use File::Slurp qw(read_file);

my $LOG_PATH = $ENV{LOG_PATH} // '/data/pixel/requests.log';
my $CERT     = $ENV{SSL_CERT}  // '/etc/ssl/certs/server.pem';
my $KEY      = $ENV{SSL_KEY}   // '/etc/ssl/private/server.key';

helper log_request => sub {
    my ($c, $data) = @_;
    eval { make_path((split('/', $LOG_PATH))[0]); };
    eval {
        open my $fh, '>>', $LOG_PATH or die $!;
        print $fh encode_json($data) . "\n";
        close $fh;
    };
};

# health check
get '/health' => sub { shift->render(text => 'ok'); };

# pixel endpoint
get '/pixel' => sub {
    my $c = shift;
    my $uid = $c->param('id') // '';
    my $record = {
        uuid        => $uid,
        ts          => strftime("%Y-%m-%dT%H:%M:%SZ", gmtime),
        remote_addr => $c->tx->remote_address // '',
        user_agent  => $c->req->headers->user_agent // '',
        referer     => $c->req->headers->header('Referer') // '',
    };
    $c->log_request($record);

    my $gif = pack('C*',
        0x47,0x49,0x46,0x38,0x39,0x61,0x01,0x00,0x01,0x00,0x80,0x00,
        0x00,0x00,0x00,0x00,0xff,0xff,0xff,0x21,0xf9,0x04,0x01,0x00,
        0x00,0x00,0x00,0x2c,0x00,0x00,0x00,0x00,0x01,0x00,0x01,0x00,
        0x00,0x02,0x02,0x44,0x01,0x00,0x3b
    );
    $c->res->headers->content_type('image/gif');
    $c->res->headers->header('Cache-Control' => 'no-cache, no-store, must-revalidate');
    $c->res->headers->header('Pragma' => 'no-cache');
    $c->res->headers->header('Expires' => '0');
    $c->render(data => $gif);
};

# reports endpoint with filters and pagination
# Query params:
#   start  - ISO datetime (inclusive) e.g. 2025-11-01T00:00:00Z
#   end    - ISO datetime (inclusive)
#   uuid   - optional UUID to filter
#   page   - page number (1-based)
#   per    - items per page (default 50)
# Examples:
#  /reports?page=1&per=20
#  /reports?start=2025-11-01T00:00:00Z&end=2025-11-10T23:59:59Z&page=2
get '/reports' => sub {
    my $c = shift;

    my $start_q = $c->param('start') // '';
    my $end_q   = $c->param('end')   // '';
    my $filter_uuid = $c->param('uuid') // '';

    my $page = $c->param('page') // 1;
    my $per  = $c->param('per')  // 50;
    $page = int($page) >= 1 ? int($page) : 1;
    $per  = int($per)  >= 1 ? int($per)  : 50;
    $per = 500 if $per > 500; # hard cap

    # Read log file safely
    my @lines;
    eval {
        @lines = map { chomp; $_ } split /\n/, read_file($LOG_PATH);
    };
    if ($@) {
        return $c->render(json => { error => "Log file not available" }, status => 503);
    }

    # parse JSON lines, filter by date/uuid
    my @items;
    for my $ln (@lines) {
        next unless $ln =~ /\S/;
        my $obj;
        eval { $obj = decode_json($ln); 1 } or next;
        # timestamps are expected in ISO UTC format "YYYY-MM-DDTHH:MM:SSZ"
        my $ts = $obj->{ts} // '';
        if ($start_q) { next if $ts lt $start_q; }
        if ($end_q)   { next if $ts gt $end_q; }
        if ($filter_uuid) { next unless defined $obj->{uuid} && $obj->{uuid} eq $filter_uuid; }
        push @items, $obj;
    }

    # sort by ts descending (most recent first)
    @items = sort { ($b->{ts} // '') cmp ($a->{ts} // '') } @items;

    # pagination
    my $total = scalar @items;
    my $pages = int( ($total + $per - 1) / $per );
    $pages = 1 if $pages < 1;
    $page = $pages if $page > $pages;
    my $start_idx = ($page - 1) * $per;
    my $paged = [ @items[ $start_idx .. ($start_idx + $per - 1) > $#items ? $#items : ($start_idx + $per - 1) ] ];

    # render JSON or simple HTML table depending on Accept header / query param
    if ($c->param('format') && $c->param('format') eq 'html' || ($c->req->headers->accept // '') =~ m{text\/html}) {
        # Build simple HTML with navigation links
        my $base_q = $c->req->url->clone->to_abs;
        $base_q->query->remove('page');
        my $html = "<html><head><meta charset='utf-8'><title>Pixel Reports</title></head><body>";
        $html .= "<h2>Pixel Reports</h2>";
        $html .= "<p>Showing page $page of $pages (total $total)</p>";
        $html .= "<table border='1' cellpadding='6' style='border-collapse:collapse;font-family:monospace;'>";
        $html .= "<thead><tr><th>ts</th><th>uuid</th><th>remote_addr</th><th>user_agent</th><th>referer</th></tr></thead><tbody>";
        for my $it (@$paged) {
            my $ts = _esc($it->{ts} // '');
            my $uuid = _esc($it->{uuid} // '');
            my $ra = _esc($it->{remote_addr} // '');
            my $ua = _esc($it->{user_agent} // '');
            my $ref = _esc($it->{referer} // '');
            $html .= "<tr><td>$ts</td><td>$uuid</td><td>$ra</td><td>$ua</td><td>$ref</td></tr>";
        }
        $html .= "</tbody></table><p>";
        # navigation links
        if ($page > 1) {
            my $prev = $base_q->clone;
            $prev->query->append(page => $page - 1);
            $html .= "<a href='$prev'>Prev</a> ";
        }
        if ($page < $pages) {
            my $next = $base_q->clone;
            $next->query->append(page => $page + 1);
            $html .= "<a href='$next'>Next</a>";
        }
        $html .= "</p></body></html>";
        return $c->render(data => $html, format => 'html');
    }

    # default JSON response
    return $c->render(json => {
        page       => $page+0,
        per        => $per+0,
        total      => $total+0,
        pages      => $pages+0,
        items      => $paged,
    });
};

# simple helper to escape HTML
sub _esc {
    my ($s) = @_;
    return '' unless defined $s;
    $s =~ s/&/&amp;/g;
    $s =~ s/</&lt;/g;
    $s =~ s/>/&gt;/g;
    $s =~ s/"/&quot;/g;
    return $s;
}

app->start('daemon', '-l', "https://*:8443", '-w', 1, '--cert', $CERT, '--key', $KEY);