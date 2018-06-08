#!/bin/sh

mysql --defaults-extra-file=/etc/mysql/debian.cnf </tmp/wordpress.sql >/dev/null 2>&1

exit 0
