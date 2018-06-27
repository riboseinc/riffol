#!/bin/sh

pidfile=/riffol/openvpn.pid
conffile=openvpn.conf

if [ "$1" = "start" ]; then
    rm -f $pidfile
    /usr/sbin/openvpn --daemon --writepid $pidfile --config $conffile
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
