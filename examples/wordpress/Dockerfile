from debian:latest

run apt-get update \
    && apt-get -y upgrade \
    && apt-get -y install --no-install-recommends wordpress mysql-server

copy apache.conf /etc/apache2/sites-available/wordpress.conf
run a2enmod rewrite && a2enmod vhost_alias && a2dissite 000-default && a2ensite wordpress

copy wordpress.php /etc/wordpress/config-default.php
copy wordpress.sql /tmp
copy initialise-mysql.sh /tmp

copy libucl.so /usr/lib/libucl.so.5
copy riffol /usr/bin
copy riffol.conf /etc

cmd riffol -f /etc/riffol.conf
