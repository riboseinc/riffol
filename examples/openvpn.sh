#!/bin/sh

NAME=riffol-openvpn
localport=1194

cd $(dirname $0)

docker build -t $NAME openvpn
docker run -t -i --rm -p $localport:1194 --cap-add=NET_ADMIN -e "NEREON_FILESET=$(base64 -w 0 <openvpn/nereond.conf)" $NAME
