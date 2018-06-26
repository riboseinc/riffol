#!/bin/sh

NAME=riffol-openvpn
localport=1194

cd $(dirname $0)

if [ "x$riffolbranch" = "x" ]; then
    riffolbranch=master
fi

if [ "x$nereondbranch" = "x" ]; then
    nereondbranch=master
fi

if [ "x$gitrebuild" = "x"]; then
    gitrebuild="no"
else
    gitrebuild="$(date)"
fi

docker build --build-arg gitrebuild="$gitrebuild" --build-arg riffolbranch=$riffolbranch --build-arg nereondbranch=$nereondbranch -t $NAME openvpn
docker run -t -i --rm -p $localport:1194 -e "NEREON_FILESET=$(base64 -w 0 <openvpn/nereond.conf)" $NAME
