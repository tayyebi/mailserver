<?php

return [

    'host' => env('MAIL_HOST', 'localhost'),

    'ports' => [
        'imap'       => (int) env('IMAP_PORT', 143),
        'imaps'      => (int) env('IMAPS_PORT', 993),
        'pop3'       => (int) env('POP3_PORT', 110),
        'pop3s'      => (int) env('POP3S_PORT', 995),
        'smtp'       => (int) env('SMTP_PORT', 25),
        'submission' => (int) env('SUBMISSION_PORT', 587),
        'smtps'      => (int) env('SMTPS_PORT', 465),
    ],

];
