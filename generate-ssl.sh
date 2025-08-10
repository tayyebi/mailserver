openssl genrsa -out ./ssl/private.key 2048
openssl rsa -in ./ssl/private.key -pubout > ./ssl/public.key
openssl req -new -x509 -key ./ssl/private.key -out ./ssl/certificate.crt -days 365

openssl dhparam -out ./ssl/dh.pem 4096

openssl req -new -x509 -days 365 -nodes -out ./ssl/dovecot.pem -keyout ./ssl/dovecot.pem.private
