pipeline {
    agent {
        dockerfile {
            dir "ci/${env.distribution}"
        }
    }
    environment {
        CARGO = "/root/.cargo/bin/cargo"
        BINARY = "target/release/bin/riffol"
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
                sh "cp ${env.BINARY} releases/${env.distribution}/"
            }
        }
    }
}
