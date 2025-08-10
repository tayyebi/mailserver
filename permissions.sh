
sudo chown -R $USER:$USER /home/ubuntu/d/mailserver/data/
find /home/ubuntu/d/mailserver/data/ -type d -exec chmod 755 {} \;
find /home/ubuntu/d/mailserver/data/ -type f -exec chmod 644 {} \;
