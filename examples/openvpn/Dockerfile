FROM centos:latest

RUN yum install -y epel-release && yum install -y openvpn sysvinit-tools
COPY riffol nereond /usr/bin/
COPY libucl.so /usr/lib
RUN ln -s /lib/libucl.so /lib/libucl.so.5; ldconfig
COPY riffol.conf /etc/
COPY openvpn-init.sh /riffol/

CMD riffol -f /etc/riffol.conf
