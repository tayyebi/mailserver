#!/usr/bin/env perl
use strict;
use warnings;

use Mojolicious::Lite;
use File::Slurp qw(read_file);
use JSON::MaybeXS;

my $DATA_DIR = "/data/pixel";

get '/' => sub {
    my $c = shift;
    $c->render(json => { status => "ok" });
};

get '/msg/:id/meta' => sub {
    my $c  = shift;
    my $id = $c->param('id');
    my $file = "$DATA_DIR/$id/meta.json";

    return $c->render(json => { error => "not found" }, status => 404)
        unless -f $file;

    $c->render(json => decode_json(read_file($file)));
};

get '/msg/:id/body' => sub {
    my $c  = shift;
    my $id = $c->param('id');
    my $file = "$DATA_DIR/$id/body.txt";

    return $c->render(text => "not found", status => 404)
        unless -f $file;

    $c->render(text => scalar read_file($file));
};

get '/msg/:id/headers' => sub {
    my $c  = shift;
    my $id = $c->param('id');
    my $file = "$DATA_DIR/$id/headers.txt";

    return $c->render(text => "not found", status => 404)
        unless -f $file;

    $c->render(text => scalar read_file($file));
};

my $CERT = $ENV{PIXEL_TLS_CERT} // "/etc/ssl/certs/pixelserver.crt";
my $KEY  = $ENV{PIXEL_TLS_KEY}  // "/etc/ssl/private/pixelserver.key";

app->start(
    'daemon',
    '-l' => "https://*:8443?cert=$CERT&key=$KEY"
);
