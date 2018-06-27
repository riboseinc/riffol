FROM centos:latest

RUN yum install -y gcc git make autoconf automake libtool epel-release sysvinit-tools \
 && yum install -y openvpn \
 && mkdir -p /dev/net && mknod /dev/net/tun c 10 200

RUN git clone https://github.com/vstakhov/libucl.git \
 && cd libucl \
 && ./autogen.sh \
 && ./configure \
 && make install

RUN curl https://sh.rustup.rs -sSf | sh -s -- -y

RUN /root/.cargo/bin/cargo install --git https://github.com/riboseinc/nereond.git
RUN /root/.cargo/bin/cargo install --git https://github.com/riboseinc/riffol.git --branch env

COPY riffol.conf /etc/
COPY openvpn-init.sh /riffol/

CMD LD_LIBRARY_PATH=/usr/local/lib /root/.cargo/bin/riffol -f /etc/riffol.conf
