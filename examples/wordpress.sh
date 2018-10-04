#!/bin/sh

NAME=riffol-wordpress

cd $(dirname $0)/..
docker build -t $NAME examples/wordpress
docker volume create --name $NAME
docker run -t -i --rm -p 8080:80 -v $NAME:/var/lib/mysql -e RUST_BACKTRACE=1 $NAME
