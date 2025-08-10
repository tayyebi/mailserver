passdb {
  driver = passwd-file
  args = scheme=SHA512-CRYPT /etc/dovecot/passwd
}
userdb {
  driver = static
  args = uid=1000 gid=1000 home=/nonexistent
}