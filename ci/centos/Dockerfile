FROM centos:latest

RUN yum install -y gcc git make autoconf automake libtool epel-release sysvinit-tools

RUN useradd -u 1000 docker-user

RUN su - docker-user -c 'curl https://sh.rustup.rs -sSf | sh -s -- -y && ~/.cargo/bin/cargo search nereon'

RUN curl -L -o/usr/bin/jq \
    https://github.com/stedolan/jq/releases/download/jq-1.5/jq-linux64 \
    && chmod +x /usr/bin/jq

RUN git clone https://github.com/vstakhov/libucl.git \
    && cd libucl \
    && ./autogen.sh \
    && ./configure --prefix=/usr \
    && make install \
    && ldconfig
