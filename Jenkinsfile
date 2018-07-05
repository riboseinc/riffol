pipeline {
    agent none
    stages {
        stage("Distros") {
            environment {
                CARGO = "/root/.cargo/bin/cargo"
                BINARY = "target/release/bin/riffol"
            }
            parallel {
                stage("Debian") {
                    environment {
                        FS_NAME = "debian"
                        POLITE_NAME = "Debian"
                    }
                    agent {
                        dockerfile {
                            dir "ci/${FS_NAME}"
                        }
                    }
                    stages {
                        stage("Test") {
                            steps {
                                sh "${CARGO} test"
                            }
                        }
                        stage("Build") {
                            steps {
                                sh "${CARGO} build --release"
                                sh "cp ${BINARY} releases/${FS_NAME}/"
                            }
                        }
                    }
                }
                stage("CentOS") {
                    environment {
                        FS_NAME = "centos"
                        POLITE_NAME = "CentOS"
                    }
                    agent {
                        dockerfile {
                            dir "ci/${FS_NAME}"
                        }
                    }
                    stages {
                        stage("Test") {
                            steps {
                                sh "${CARGO} test"
                            }
                        }
                        stage("Build") {
                            steps {
                                sh "${CARGO} build --release"
                                sh "cp ${BINARY} releases/${FS_NAME}/"
                            }
                        }
                    }
                }
            }
        }
    }
}