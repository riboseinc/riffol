#!/bin/sh

NAME=riffol-openvpn
localport=1194

cd $(dirname $0)

docker build --build-arg riffolbranch=$riffolbranch --build-arg nereondbranch=$nereondbranch -t $NAME openvpn
docker run -t -i --rm -p $localport:1194 -e "NEREON_FILESET=$(base64 -w 0 <openvpn/nereond.conf)" $NAME
