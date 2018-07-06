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
                    stages {
                        stage("Test") {
                            steps {
                                sh "${env.CARGO} test"
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
                                sh "${env.CARGO} clean"
                                sh "${env.CARGO} test"
                            }
                        }
                    }
                }
            }
        }
    }
}