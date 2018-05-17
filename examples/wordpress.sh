#!/bin/sh

cd $(dirname $0)/..
cargo build --release
cp target/release/riffol examples/wordpress/
docker build -t wordpress examples/wordpress
docker run -t -i --rm -p 8080:80 wordpress
