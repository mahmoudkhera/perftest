# perftest

[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)  

`perftest` is a **high-performance TCP benchmarking tool** written in Rust.  
It allows you to measure **throughput, latency, and stability** of network connections, inspired by tools like `iperf3`, but built in Rust for **speed, safety, and modern async design**.

# Acknowledgments
at first thanks to egneer Ahmed soliman for his excelent tutoriala in youtube that helped me a lot 
and for his reverse engineering project that i write my on netwrok perforemec tool based on it but i was  learngin so 
i decided to built again but in my own style for learning 


At first, I would like to thank [Eng. Ahmed Soliman](https://github.com/ahmedsoliman)
for his excellent tutorial on [YouTube](https://www.youtube.com/c/ahmedsoliman](https://youtube.com/playlist?list=PLald6EODoOJU0GMuYHlkS9MLhTPE7HiaT&si=KggPmyixF4vktXOi)) that helped me a lot,
and for his reverse engineering repo for i perf3 , which inspired me to write my own network performance tool.
Since I was learning, I decided to rebuild the tool in my own style as part of my learning journey.


## ✨ Features

-  Measure TCP throughput between a client and server  
-  Support for multiple parallel streams (`-P`)  
-  Efficient design using Rust async/runtime    
-  Memory-safe implementation compared to legacy C tools  

---
 #  Limitations
No UDP/STCP support yet.

