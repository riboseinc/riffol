pipeline {
    agent none
    stages {
        stage("Distros") {
            agent none
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
                            dir "ci/${env.FS_NAME}"
                        }
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
                    environment {
                        FS_NAME = "centos"
                        POLITE_NAME = "CentOS"
                    }
                    agent {
                        dockerfile {
                            dir "ci/${env.FS_NAME}"
                        }
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