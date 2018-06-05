from debian:latest

run apt-get update \
    && apt-get -y upgrade \
    && apt-get -y install --no-install-recommends wordpress mysql-server

copy apache.conf /etc/apache2/sites-available/wordpress.conf
copy wordpress.php /etc/wordpress/config-default.php
copy wordpress.sql /tmp

run /etc/init.d/mysql start && mysql --defaults-extra-file=/etc/mysql/debian.cnf </tmp/wordpress.sql
run a2enmod rewrite && a2enmod vhost_alias && a2dissite 000-default && a2ensite wordpress

copy riffol.conf /etc
copy riffol /usr/bin

cmd riffol /etc/riffol.conf