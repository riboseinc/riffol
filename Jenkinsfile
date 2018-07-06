pipeline {
    agent none
    stages {
        stage("Distros") {
            environment {
                CARGO = "~/.cargo/bin/cargo"
                BINARY = "target/release/bin/riffol"
                LD_LIBRARY_PATH = "/usr/local/bin" // libucl.so
            }
            stages {
                stage("Debian") {
                    agent {
                        dockerfile {
                            dir "ci/debian"
                        }
                    }
                    stages {
                        stage("Test") {
                            steps {
                                sh "${env.CARGO} test"
                            }
                        }
                    }
                }
                stage("CentOS") {
                    agent {
                        dockerfile {
                            dir "ci/centos"
                        }
                    }
                    stages {
                        stage("Test") {
                            steps {
                                sh "${env.CARGO} test"
                            }
                        }
                    }
                }
            }
        }
    }
}