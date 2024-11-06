Perf notes:
- EntityLocations splits generations and locations into separate vectors
  - cache miss when accessing both
- HashMap for table registry still has indirection
- Vec<Option<(usize, usize)>> for locations causes scattered memory access
- Components being split across many tables fragments cache lines
- Batch operations still do single operations in a loop
- Parallel operations are adding more overhead than sequential processing until very large entity counts
- Table fragmentation optimizations could be added to improve cache locality
- Look into periodic performance spikes during extreme benchmark frames
- Optimize the replace operation, as it's significantly slower than other operations