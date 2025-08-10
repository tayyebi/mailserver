for domain in gordarg.com tyyi.net 0pt.ir; do
  echo "DKIM record for $domain:"
  cat data/opendkim/keys/$domain/dkim.txt
done
