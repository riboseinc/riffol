#!/bin/sh

sleep 5
mysql --defaults-extra-file=/etc/mysql/debian.cnf </tmp/wordpress.sql >/dev/null 2>&1
