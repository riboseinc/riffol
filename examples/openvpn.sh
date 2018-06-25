#!/bin/sh

NAME=riffol-openvpn
LOCALPORT=1194

cd $(dirname $0)

docker build -t $NAME openvpn
docker run -t -i --rm -p $LOCAL_PORT:1194 -e "NEREON_FILESET=$(base64 -w 0 <openvpn/nereond.conf)" $NAME
