# FoundationDB Parallel Executor

This is a benchmark of the FoundationDB Parallel Executor with different thread counts.

```
════════════════════════════════════════════════════════════════════════════════════════════════════════════
  FoundationDB Parallel Executor (Multi-threaded with automatic retry)
════════════════════════════════════════════════════════════════════════════════════════════════════════════

--- 1 threads ---
Config               | Executor             | Ordering | Successful | Failed     | Time (ms)    | TPS         
-------------------------------------------------------------------------------------------------------------------
No conflicts         | fdb_parallel         | loose    | 2500       | 0          | 5499.87      | 455         
25% conflicts        | fdb_parallel         | loose    | 2500       | 0          | 5963.00      | 419         
50% conflicts        | fdb_parallel         | loose    | 2500       | 0          | 3832.04      | 652         
75% conflicts        | fdb_parallel         | loose    | 2500       | 0          | 3687.49      | 678         
Full conflicts       | fdb_parallel         | loose    | 2500       | 0          | 3989.41      | 627         

--- 2 threads ---
Config               | Executor             | Ordering | Successful | Failed     | Time (ms)    | TPS         
-------------------------------------------------------------------------------------------------------------------
No conflicts         | fdb_parallel         | loose    | 2488       | 12         | 2422.65      | 1032        
25% conflicts        | fdb_parallel         | loose    | 2487       | 13         | 2698.15      | 927         
50% conflicts        | fdb_parallel         | loose    | 2482       | 18         | 2584.20      | 967         
75% conflicts        | fdb_parallel         | loose    | 2454       | 46         | 2642.04      | 946         
Full conflicts       | fdb_parallel         | loose    | 1250       | 1250       | 2064.00      | 1211        

--- 4 threads ---
Config               | Executor             | Ordering | Successful | Failed     | Time (ms)    | TPS         
-------------------------------------------------------------------------------------------------------------------
No conflicts         | fdb_parallel         | loose    | 2476       | 24         | 1693.11      | 1477        
25% conflicts        | fdb_parallel         | loose    | 2478       | 22         | 1732.65      | 1443        
50% conflicts        | fdb_parallel         | loose    | 2477       | 23         | 1692.15      | 1477        
75% conflicts        | fdb_parallel         | loose    | 2425       | 75         | 1682.43      | 1486        
Full conflicts       | fdb_parallel         | loose    | 625        | 1875       | 1462.68      | 1709        

--- 8 threads ---
Config               | Executor             | Ordering | Successful | Failed     | Time (ms)    | TPS         
-------------------------------------------------------------------------------------------------------------------
No conflicts         | fdb_parallel         | loose    | 2473       | 27         | 1171.11      | 2135        
25% conflicts        | fdb_parallel         | loose    | 2471       | 29         | 1075.00      | 2326        
50% conflicts        | fdb_parallel         | loose    | 2467       | 33         | 1087.81      | 2298        
75% conflicts        | fdb_parallel         | loose    | 2426       | 74         | 1083.54      | 2307        
Full conflicts       | fdb_parallel         | loose    | 313        | 2187       | 967.90       | 2583        
```

Avoiding balance issues, nonce is retried smartly:

```
════════════════════════════════════════════════════════════════════════════════════════════════════════════
  FoundationDB Parallel Executor (Multi-threaded with automatic retry)
════════════════════════════════════════════════════════════════════════════════════════════════════════════

--- 1 threads ---
Config               | Executor             | Ordering | Successful | Failed     | Time (ms)    | TPS         
-------------------------------------------------------------------------------------------------------------------
No conflicts         | fdb_parallel         | loose    | 2500       | 0          | 3460.97      | 722         
25% conflicts        | fdb_parallel         | loose    | 2500       | 0          | 3577.56      | 699         
50% conflicts        | fdb_parallel         | loose    | 2500       | 0          | 4167.27      | 600         
75% conflicts        | fdb_parallel         | loose    | 2500       | 0          | 3328.93      | 751         
Full conflicts       | fdb_parallel         | loose    | 2500       | 0          | 3412.36      | 733         

--- 4 threads ---
Config               | Executor             | Ordering | Successful | Failed     | Time (ms)    | TPS         
-------------------------------------------------------------------------------------------------------------------
No conflicts         | fdb_parallel         | loose    | 2500       | 0          | 3600.35      | 694         
25% conflicts        | fdb_parallel         | loose    | 2500       | 0          | 2639.00      | 947         
50% conflicts        | fdb_parallel         | loose    | 2500       | 0          | 2549.58      | 981         
75% conflicts        | fdb_parallel         | loose    | 2500       | 0          | 3388.42      | 738         
Full conflicts       | fdb_parallel         | loose    | 2500       | 0          | 3333.40      | 750         

--- 16 threads ---
Config               | Executor             | Ordering | Successful | Failed     | Time (ms)    | TPS         
-------------------------------------------------------------------------------------------------------------------
No conflicts         | fdb_parallel         | loose    | 2500       | 0          | 1625.97      | 1538        
25% conflicts        | fdb_parallel         | loose    | 2500       | 0          | 1403.17      | 1782        
50% conflicts        | fdb_parallel         | loose    | 2500       | 0          | 1573.19      | 1589        
75% conflicts        | fdb_parallel         | loose    | 2500       | 0          | 1959.47      | 1276        
Full conflicts       | fdb_parallel         | loose    | 2500       | 0          | 4330.12      | 577         

--- 64 threads ---
Config               | Executor             | Ordering | Successful | Failed     | Time (ms)    | TPS         
-------------------------------------------------------------------------------------------------------------------
No conflicts         | fdb_parallel         | loose    | 2500       | 0          | 933.36       | 2679        
25% conflicts        | fdb_parallel         | loose    | 2500       | 0          | 921.21       | 2714        
50% conflicts        | fdb_parallel         | loose    | 2500       | 0          | 941.59       | 2655        
75% conflicts        | fdb_parallel         | loose    | 2500       | 0          | 962.28       | 2598        
Full conflicts       | fdb_parallel         | loose    | 2500       | 0          | 7180.93      | 348         
```