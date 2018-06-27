#!/bin/sh

NAME=riffol-openvpn
localport=1194

cd $(dirname $0)

serial=$(cat openvpn/serial)

if [ "x$riffolbranch" = "x" ]; then
    riffolbranch=master
fi

if [ "x$nereondbranch" = "x" ]; then
    nereondbranch=master
fi

docker build --build-arg serial=$serial --build-arg riffolbranch=$riffolbranch --build-arg nereondbranch=$nereondbranch -t $NAME openvpn
docker run -t -i --rm -p $localport:1194 --cap-add=NET_ADMIN -e "NEREON_FILESET=$(base64 -w 0 <openvpn/nereond.conf)" $NAME
