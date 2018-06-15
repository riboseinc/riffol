#!/bin/sh

pidfile=/riffol/openvpn.pid
logfile=/riffol/openvpn.log
conffile=/riffol/server.conf

cd $(dirname $0)

umask 0077

if [ "$1" = "start" ]; then
    echo $RIFFOL_OPENVPN_CONF | base64 -d >server.conf
    echo $RIFFOL_OPENVPN_CA | base64 -d >ca.crt
    echo $RIFFOL_OPENVPN_CERT | base64 -d >server.crt
    echo $RIFFOL_OPENVPN_KEY | base64 -d >server.key
    echo $RIFFOL_OPENVPN_DH | base64 -d >dh2048.pem
    echo $RIFFOL_OPENVPN_TA | base64 -d >ta.key

    mkdir -p /dev/net
    if [ ! -c /dev/net/tun ]; then
        mknod /dev/net/tun c 10 200
    fi

    rm -f $pidfile
    /usr/sbin/openvpn --daemon --writepid $pidfile --log $logfile --config $conffile
fi

if [ "$1" = "stop" ]; then
    if [ -e $pidfile ]; then
        kill -TERM $(cat $pidfile)
        rm $pidfile
    fi
fi

if [ "$1" = "restart" ]; then
    if [ -e $pidfile ]; then
        kill -HUP $(cat $pidfile)
    fi
fi
