pipeline {
    agent none
    stages {
        stage("Test and Build") {
            environment {
                CARGO = "~/.cargo/bin/cargo"
                OAUTH = credentials("GitHub")
            }
            when {
                 branch 'master'
            }
            stages {
                stage("Debian") {
                    agent {
                        dockerfile {
                            dir "ci/debian"
                        }
                    }
                    steps {
                        sh """
                            $CARGO clean
                            $CARGO update
                            $CARGO test
                            $CARGO build --release
                        """
                        sh '''
                            LIBC_VERSION=$(ldd --version | head -n1 | sed -r 's/(.* )//')
                            mkdir -p assets
                            tar -C target/release -zcvf assets/riffol-libc-$LIBC_VERSION.tar.gz riffol
                            ci/release.sh riboseinc/riffol
                        '''
                    }
                }
                stage("CentOS") {
                    agent {
                        dockerfile {
                            dir "ci/centos"
                        }
                    }
                    steps {
                        sh """
                            $CARGO clean
                            $CARGO update
                            $CARGO test
                            $CARGO build --release
                        """
                        sh '''
                            LIBC_VERSION=$(ldd --version | head -n1 | sed -r 's/(.* )//')
                            mkdir -p assets
                            tar -C target/release -zcvf assets/riffol-libc-$LIBC_VERSION.tar.gz riffol
                            ci/release.sh riboseinc/riffol
                        '''
                    }
                }
            }
        }
    }
}