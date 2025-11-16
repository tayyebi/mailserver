#!/usr/bin/env perl
use strict;
use warnings;

use Mail::Milter::Server;
use Mail::Milter::Context qw(SMFIS_CONTINUE SMFIS_ACCEPT);
use MIME::Parser;
use JSON::MaybeXS;
use Data::UUID;
use File::Slurp qw(write_file);
use File::Path qw(make_path);
use POSIX qw(strftime);

###############################################
# Per-message milter filter object
###############################################

{
    package PixelFilter;
    use parent 'Mail::Milter::Filter';

    sub new {
        my ($class) = @_;
        return bless {}, $class;
    }

    sub envfrom {
        my ($self, $ctx, $sender) = @_;
        $self->{id}       = _generate_id();
        $self->{headers}  = "";
        $self->{body}     = "";
        return SMFIS_CONTINUE;
    }

    sub header {
        my ($self, $ctx, $name, $value) = @_;
        $self->{headers} .= "$name: $value\n";
        return SMFIS_CONTINUE;
    }

    sub eoh {
        my ($self, $ctx) = @_;
        $self->{headers} .= "\n";
        return SMFIS_CONTINUE;
    }

    sub body {
        my ($self, $ctx, $chunk) = @_;
        $self->{body} .= $chunk;
        return SMFIS_CONTINUE;
    }

    sub eom {
        my ($self, $ctx) = @_;

        my $id   = $self->{id};
        my $base = "/data/pixel/$id";

        make_path($base);

        write_file("$base/headers.txt", $self->{headers});
        write_file("$base/body.txt",    $self->{body});
        write_file("$base/meta.json", encode_json({
            id      => $id,
            created => scalar localtime,
            size    => length($self->{headers}) + length($self->{body}),
        }));

        return SMFIS_ACCEPT;
    }
}

###############################################
# Utilities
###############################################

sub _generate_id {
    my $ug = Data::UUID->new;
    return strftime("%Y%m%d-%H%M%S", localtime) . "-" . $ug->create_str;
}

###############################################
# Run the milter server
###############################################

my $socket = $ENV{PIXEL_MILTER_SOCKET} // "/var/run/pixelmilter/pixel.sock";
unlink $socket if -e $socket;

my $server = Mail::Milter::Server->new({
    unix => $socket,
});

$server->register('pixel' => PixelFilter->new());

$server->main();
