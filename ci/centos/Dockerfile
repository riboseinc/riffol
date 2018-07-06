FROM centos:latest

RUN yum install -y gcc git make autoconf automake libtool epel-release sysvinit-tools

RUN curl https://sh.rustup.rs -sSf | sh -s -- -y

RUN git clone https://github.com/vstakhov/libucl.git \
    && cd libucl \
    && ./autogen.sh \
    && ./configure \
    && make install
