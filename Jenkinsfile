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
                    agent {
                        dockerfile {
                            dir "ci/debian"
                        }
                    }
                    environment {
                        FS_NAME = "debian"
                        POLITE_NAME = "Debian"
                    }
                    stages {
                        stage("Test") {
                            steps {
                                sh "${env.CARGO} test"
                            }
                        }
                        stage("Build") {
                            steps {
                                sh "${env.CARGO} build --release"
                                sh "cp ${env.BINARY} releases/${env.FS_NAME}/"
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
                    environment {
                        FS_NAME = "centos"
                        POLITE_NAME = "CentOS"
                    }
                    stages {
                        stage("Test") {
                            steps {
                                sh "${env.CARGO} test"
                            }
                        }
                        stage("Build") {
                            steps {
                                sh "${env.CARGO} build --release"
                                sh "cp ${env.BINARY} releases/${env.FS_NAME}/"
                            }
                        }
                    }
                }
            }
        }
    }
}