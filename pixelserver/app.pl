#!/usr/bin/env perl
use strict;
use warnings;

use Mojolicious::Lite;
use File::Slurp qw(read_file write_file);
use JSON::MaybeXS;
use POSIX qw(strftime);
use File::Path qw(make_path);
use Encode qw(encode);

my $DATA_DIR = $ENV{DATA_DIR} // "/data/pixel";
my $LOG_PATH = $ENV{LOG_PATH} // "/data/pixel/requests.log";

# 1x1 transparent GIF in base64
my $PIXEL_GIF = pack("H*", "47494638396101000100800000000000ffffff21f90401000000002c00000000010001000002024401003b");

# Logging utility
sub log_event {
    my ($message) = @_;
    my $timestamp = strftime("%Y-%m-%d %H:%M:%S", localtime);
    my $log_entry = "[$timestamp] $message\n";
    
    eval {
        # Ensure log directory exists
        my $log_dir = $LOG_PATH;
        $log_dir =~ s{/[^/]+$}{};
        make_path($log_dir) if $log_dir && !-d $log_dir;
        
        if (open my $fh, '>>', $LOG_PATH) {
            print $fh $log_entry;
            close $fh;
        }
    };
    
    app->log->info($message);
}

# Update message metadata with tracking info
sub update_tracking {
    my ($id, $client_ip, $user_agent) = @_;
    
    my $meta_file = "$DATA_DIR/$id/meta.json";
    return unless -f $meta_file;
    
    eval {
        my $meta = decode_json(read_file($meta_file));
        my $now = time();
        
        $meta->{opened} = JSON::MaybeXS::true;
        $meta->{open_count}++;
        $meta->{last_open} = $now;
        $meta->{last_open_str} = scalar localtime($now);
        
        unless ($meta->{first_open}) {
            $meta->{first_open} = $now;
            $meta->{first_open_str} = scalar localtime($now);
        }
        
        # Add tracking event
        push @{$meta->{tracking_events} ||= []}, {
            timestamp => $now,
            timestamp_str => scalar localtime($now),
            client_ip => $client_ip,
            user_agent => $user_agent,
        };
        
        write_file($meta_file, encode_json($meta));
        log_event("Tracking updated for message $id (opens: $meta->{open_count})");
    };
}

# Health check endpoint
get '/health' => sub {
    my $c = shift;
    
    my $status = {
        status => "ok",
        timestamp => time(),
        data_dir => $DATA_DIR,
        data_dir_exists => -d $DATA_DIR ? JSON::MaybeXS::true : JSON::MaybeXS::false,
        data_dir_writable => -w $DATA_DIR ? JSON::MaybeXS::true : JSON::MaybeXS::false,
    };
    
    $c->render(json => $status);
};

# Main status endpoint
get '/' => sub {
    my $c = shift;
    $c->render(json => { 
        status => "ok", 
        service => "pixelserver",
        version => "1.0.0",
        timestamp => time()
    });
};

# Pixel tracking endpoint - serves 1x1 transparent GIF
get '/pixel' => sub {
    my $c = shift;
    my $id = $c->param('id');
    
    unless ($id) {
        log_event("Pixel request without ID from " . ($c->tx->remote_address || 'unknown'));
        return $c->render(data => $PIXEL_GIF, format => 'gif');
    }
    
    # Validate ID format (basic security)
    unless ($id =~ /^[0-9]{8}-[0-9]{6}-[A-F0-9-]+$/i) {
        log_event("Invalid pixel ID format: $id from " . ($c->tx->remote_address || 'unknown'));
        return $c->render(data => $PIXEL_GIF, format => 'gif');
    }
    
    my $client_ip = $c->tx->remote_address || 'unknown';
    my $user_agent = $c->req->headers->user_agent || 'unknown';
    
    log_event("Pixel request for ID: $id from $client_ip");
    
    # Update tracking data
    update_tracking($id, $client_ip, $user_agent);
    
    # Always return the pixel regardless of whether tracking succeeded
    $c->res->headers->cache_control('no-cache, no-store, must-revalidate');
    $c->res->headers->pragma('no-cache');
    $c->res->headers->expires('0');
    $c->render(data => $PIXEL_GIF, format => 'gif');
};

# Message metadata endpoint
get '/msg/:id/meta' => sub {
    my $c  = shift;
    my $id = $c->param('id');
    
    # Validate ID
    unless ($id =~ /^[0-9]{8}-[0-9]{6}-[A-F0-9-]+$/i) {
        return $c->render(json => { error => "invalid id format" }, status => 400);
    }
    
    my $file = "$DATA_DIR/$id/meta.json";
    
    return $c->render(json => { error => "not found" }, status => 404)
        unless -f $file;
    
    eval {
        my $meta = decode_json(read_file($file));
        $c->render(json => $meta);
    } or do {
        log_event("Error reading metadata for $id: $@");
        $c->render(json => { error => "internal error" }, status => 500);
    };
};

# Message body endpoint
get '/msg/:id/body' => sub {
    my $c  = shift;
    my $id = $c->param('id');
    
    # Validate ID
    unless ($id =~ /^[0-9]{8}-[0-9]{6}-[A-F0-9-]+$/i) {
        return $c->render(text => "Invalid ID format", status => 400);
    }
    
    my $file = "$DATA_DIR/$id/body.txt";
    
    return $c->render(text => "Not found", status => 404)
        unless -f $file;
    
    eval {
        $c->render(text => scalar read_file($file));
    } or do {
        log_event("Error reading body for $id: $@");
        $c->render(text => "Internal error", status => 500);
    };
};

# Message headers endpoint
get '/msg/:id/headers' => sub {
    my $c  = shift;
    my $id = $c->param('id');
    
    # Validate ID
    unless ($id =~ /^[0-9]{8}-[0-9]{6}-[A-F0-9-]+$/i) {
        return $c->render(text => "Invalid ID format", status => 400);
    }
    
    my $file = "$DATA_DIR/$id/headers.txt";
    
    return $c->render(text => "Not found", status => 404)
        unless -f $file;
    
    eval {
        $c->render(text => scalar read_file($file));
    } or do {
        log_event("Error reading headers for $id: $@");
        $c->render(text => "Internal error", status => 500);
    };
};

# Statistics endpoint
get '/stats' => sub {
    my $c = shift;
    
    my $stats = {
        total_messages => 0,
        tracked_messages => 0,
        opened_messages => 0,
        total_opens => 0,
    };
    
    eval {
        opendir(my $dh, $DATA_DIR) or die "Cannot open $DATA_DIR: $!";
        while (my $entry = readdir($dh)) {
            next if $entry =~ /^\./ || !-d "$DATA_DIR/$entry";
            
            my $meta_file = "$DATA_DIR/$entry/meta.json";
            next unless -f $meta_file;
            
            $stats->{total_messages}++;
            
            my $meta = decode_json(read_file($meta_file));
            if ($meta->{tracking_enabled}) {
                $stats->{tracked_messages}++;
                if ($meta->{opened}) {
                    $stats->{opened_messages}++;
                    $stats->{total_opens} += ($meta->{open_count} || 0);
                }
            }
        }
        closedir($dh);
    };
    
    $c->render(json => $stats);
};

# Ensure data directory exists
make_path($DATA_DIR) unless -d $DATA_DIR;

# Log startup
log_event("Pixelserver starting up, data directory: $DATA_DIR");

my $CERT = $ENV{PIXEL_TLS_CERT} // "/etc/ssl/certs/server.pem";
my $KEY  = $ENV{PIXEL_TLS_KEY}  // "/etc/ssl/private/server.key";

# Configure Mojolicious
app->config(
    hypnotoad => {
        listen => ["https://*:8443?cert=$CERT&key=$KEY"],
        workers => 4,
        graceful_timeout => 10,
    }
);

log_event("Pixelserver ready to serve requests");

app->start(
    'daemon',
    '-l' => "https://*:8443?cert=$CERT&key=$KEY"
);
