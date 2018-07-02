#!/bin/sh

NAME=riffol-wordpress
UCLDIR=/usr/local/lib

cd $(dirname $0)/..
docker build -t $NAME examples/wordpress
docker volume create --name $NAME
docker run -t -i --rm -p 8080:80 -v $NAME:/var/lib/mysql $NAME
