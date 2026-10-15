# 验证记录

环境：2026-10-15，Linux x86_64，GCC 15.2.0，CMake 4.2.3，Python 3.14.4。
上游固定：c9c5979f04cea8c38213004d4f04df0dcfb40d05。

命令：

```sh
cmake -S . -B build -G Ninja
cmake --build build -j 6
ctest --test-dir build --output-on-failure
./build/kernel_probe
```

结果：独立内核编译成功，CTest 1/1 通过（0.02 秒）。

```text
box: volume=1000 edges=12 valid=true
boolean 0: volume=1500 valid=true
boolean 1: volume=500 valid=true
boolean 2: volume=500 valid=true
fillet: volume=997.854 valid=true source_unchanged=true
tessellation: vertices=16
```

体积核对使用解析期望值与显式容差；Release 下不使用会被 NDEBUG 去除的 assert。
首次编译暴露探针调用 SmTArray::Append 的类型错误，已在自有代码改用 Add，内核源码未变。
STEP 不属于本探针支持范围；不能声明完整八步工作流已通过。
