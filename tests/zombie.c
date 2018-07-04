#include <unistd.h>

int main(void) {
  if (!fork()){
    if (!fork()){
      sleep(5);
    }
  }
}
