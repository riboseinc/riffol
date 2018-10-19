FROM centos:latest

RUN yum install -y epel-release \
 && yum install -y curl gcc libc6-dev \
 && curl https://sh.rustup.rs -sSf | sh -s -- -y

RUN /root/.cargo/bin/cargo install nereond --git https://github.com/riboseinc/nereond.git --rev 25b97fa
RUN /root/.cargo/bin/cargo install riffol --git https://github.com/riboseinc/riffol.git --rev b22aa3
RUN /root/.cargo/bin/cargo install nereond --force --git https://github.com/riboseinc/nereond.git --rev af0f72c

COPY riffol.conf /etc/
COPY openvpn-init.sh /riffol/

ENTRYPOINT ["/root/.cargo/bin/riffol",  "-f",  "/etc/riffol.conf"]
