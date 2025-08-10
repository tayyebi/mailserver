for domain in gordarg.com tyyi.net 0pt.ir; do
  mkdir -p data/opendkim/keys/$domain
  opendkim-genkey -D data/opendkim/keys/$domain/ -d $domain -s dkim
  chown -R 1000:1000 data/opendkim/keys/$domain
done
