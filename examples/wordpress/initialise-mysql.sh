#!/bin/sh

if [ "$1" = "start" ]; then
    mysql --defaults-extra-file=/etc/mysql/debian.cnf </tmp/wordpress.sql >/dev/null 2>&1
fi

exit 0
