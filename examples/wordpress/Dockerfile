FROM debian:latest

RUN apt-get update \
    && apt-get -y upgrade \
    && apt-get -y install --no-install-recommends wordpress wordpress-theme-twentyseventeen mysql-server \
    && apt-get -y install --no-install-recommends autoconf automake libtool make pkg-config git curl

COPY apache.conf /etc/apache2/sites-available/wordpress.conf
RUN a2enmod rewrite && a2enmod vhost_alias && a2dissite 000-default && a2ensite wordpress

COPY wordpress.php /etc/wordpress/config-default.php
COPY wordpress.sql /tmp
COPY initialise-mysql.sh /tmp

RUN git clone https://github.com/vstakhov/libucl.git \
 && cd libucl \
 && ./autogen.sh \
 && ./configure \
 && make install

RUN curl https://sh.rustup.rs -sSf | sh -s -- -y

RUN /root/.cargo/bin/cargo install --git https://github.com/riboseinc/riffol.git

COPY riffol.conf /etc

CMD LD_LIBRARY_PATH=/usr/local/lib /root/.cargo/bin/riffol -f /etc/riffol.conf
