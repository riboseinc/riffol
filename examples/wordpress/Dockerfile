FROM debian:latest

RUN apt-get update \
    && apt-get -y upgrade \
    && apt-get -y install --no-install-recommends \
        curl \
        mysql-server \
        wordpress \
        wordpress-theme-twentyseventeen \
    && curl https://sh.rustup.rs -sSf | sh -s -- -y

RUN /root/.cargo/bin/cargo install riffol

COPY apache.conf /etc/apache2/sites-available/wordpress.conf
RUN a2enmod rewrite && a2enmod vhost_alias && a2dissite 000-default && a2ensite wordpress

COPY wordpress.php /etc/wordpress/config-default.php
COPY wordpress.sql /tmp
COPY initialise-mysql.sh /tmp
COPY riffol.conf /etc

CMD /root/.cargo/bin/riffol -f /etc/riffol.conf
