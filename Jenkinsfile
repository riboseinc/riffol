pipeline {
    agent none
    stages {
        stage("Distros") {
            environment {
                CARGO = "~/.cargo/bin/cargo"
                BINARY = "target/release/bin/riffol"
            }
            stages {
                stage("Debian") {
                    agent {
                        dockerfile {
                            dir "ci/debian"
                        }
                    }
                    steps {
                        sh "${env.CARGO} clean && ${env.CARGO} update && ${env.CARGO} test"
                    }
                }
                stage("CentOS") {
                    agent {
                        dockerfile {
                            dir "ci/centos"
                        }
                    }
                    steps {
                        sh "${env.CARGO} clean && ${env.CARGO} update && ${env.CARGO} test"
                    }
                }
            }
        }
    }
}