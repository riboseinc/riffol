FROM debian:latest

RUN apt-get update \
    && apt-get -y upgrade \
    && apt-get -y install --no-install-recommends autoconf automake libtool make pkg-config \
       git curl ca-certificates

RUN useradd -u 1000 docker-user && \
    mkdir -p /home/docker-user && \
    chown docker-user:docker-user /home/docker-user

RUN su - docker-user -c 'curl https://sh.rustup.rs -sSf | sh -s -- -y && ~/.cargo/bin/cargo search nereon'

RUN git clone https://github.com/vstakhov/libucl.git \
    && cd libucl \
    && ./autogen.sh \
    && ./configure --prefix=/usr \
    && make install
