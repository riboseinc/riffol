pipeline {
    agent none
    stages {
        stage("Test and Build") {
            environment {
                CARGO = "~/.cargo/bin/cargo"
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
                            tar -C target/release -czf assets/riffol-$LIBC_VERSION.tar.gz riffol
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
                            tar -C target/release -czf assets/riffol-$LIBC_VERSION.tar.gz riffol
                        '''
                    }
                }
            }
        }
        stage("Upload Assets") {
            agent any
            when {
                 branch 'master'
            }
            environment {
                OAUTH = credentials("GitHub")
            }
            steps {
                sh '''
                    curl -L -o/usr/bin/jq https://github.com/stedolan/jq/releases/download/jq-1.5/jq-linux64
                    chmod +x /usr/bin/jq
                    ci/release.sh riboseinc/riffol
                '''
            }
        }
    }
}