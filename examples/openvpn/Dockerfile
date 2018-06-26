from centos:latest

cmd LD_LIBRARY_PATH=/usr/local/lib /root/.cargo/bin/riffol -f /etc/riffol.conf

run yum install -y gcc git make autoconf automake libtool
run git clone https://github.com/vstakhov/libucl.git && cd libucl && ./autogen.sh && ./configure && make install

run yum -y install epel-release sysvinit-tools
run yum -y install openvpn
run mkdir -p /dev/net && mknod /dev/net/tun c 10 200

run curl https://sh.rustup.rs -sSf | sh -s -- -y
run /root/.cargo/bin/cargo install --git https://github.com/riboseinc/riffol.git/$riffolbranch
run /root/.cargo/bin/cargo install --git https://github.com/riboseinc/nereond.git/$nereondbranch

copy riffol.conf /etc/
copy openvpn-init.sh /riffol/

