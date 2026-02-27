package com.miti99;

import java.time.LocalDateTime;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;

public class Main {

  static void main() throws InterruptedException {
    var executor = Executors.newSingleThreadScheduledExecutor();
    executor.scheduleAtFixedRate(() -> IO.println(LocalDateTime.now()), 1, 1, TimeUnit.SECONDS);
    Runtime.getRuntime().addShutdownHook(new Thread(executor::shutdown));
    Thread.currentThread().join();
  }
}
