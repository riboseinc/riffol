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
                    stages {
                        stage("Test") {
                            agent {
                                dockerfile {
                                    dir "ci/${FS_NAME}"
                                }
                            }
                            steps {
                                sh "${CARGO} test"
                            }
                        }
                        stage("Build") {
                            agent {
                                dockerfile {
                                    dir "ci/${FS_NAME}"
                                }
                            }
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
                    stages {
                        stage("Test") {
                            agent {
                                dockerfile {
                                    dir "ci/${env.FS_NAME}"
                                }
                            }
                            steps {
                                sh "${env.CARGO} test"
                            }
                        }
                        stage("Build") {
                            agent {
                                dockerfile {
                                    dir "ci/${env.FS_NAME}"
                                }
                            }
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