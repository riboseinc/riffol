FROM debian:latest

RUN apt-get update \
    && apt-get -y upgrade \
    && apt-get -y install --no-install-recommends autoconf automake libtool make pkg-config \
       git curl ca-certificates

RUN curl https://sh.rustup.rs -sSf | sh -s -- -y

RUN git clone https://github.com/vstakhov/libucl.git \
    && cd libucl \
    && ./autogen.sh \
    && ./configure \
    && make install
