FROM centos:latest

RUN yum install -y epel-release \
 && yum install -y curl gcc libc6-dev openvpn \
 && curl https://sh.rustup.rs -sSf | sh -s -- -y

RUN /root/.cargo/bin/cargo install nereond
RUN /root/.cargo/bin/cargo install riffol --git https://github.com/riboseinc/riffol.git --rev f78fb6b

COPY riffol.conf /etc/
COPY openvpn-init.sh /riffol/

ENTRYPOINT ["/root/.cargo/bin/riffol",  "-f",  "/etc/riffol.conf"]
