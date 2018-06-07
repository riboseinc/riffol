#!/bin/sh

NAME=riffol-wordpress
UCLDIR=/usr/local/lib

cd $(dirname $0)/..
cargo build --release
cp target/release/riffol examples/wordpress/
cp $UCLDIR/libucl.so examples/wordpress/libucl.so
docker build -t $NAME examples/wordpress
docker volume create --name $NAME
docker run -t -i --rm -p 8080:80 -v $NAME:/var/lib/mysql $NAME
