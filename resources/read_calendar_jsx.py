import os

with open(r"d:\capcutpolot\postiz-app\apps\frontend\src\components\launches\calendar.tsx", "r", encoding="utf-8") as f:
    txt = f.read()

# In ra phan JSX render calendar
idx = txt.find("return (")
print("Found return at:", idx)
print(txt[idx:idx+1500])

