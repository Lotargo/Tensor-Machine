import csv
import random

# G = 1 for normalized symbolic regression task
# F = (m1 * m2) / (r * r)

with open('v7/engine/gravity_data.csv', 'w', newline='') as csvfile:
    writer = csv.writer(csvfile)
    for _ in range(20): # 20 data points
        m1 = round(random.uniform(1.0, 10.0), 2)
        m2 = round(random.uniform(1.0, 10.0), 2)
        r = round(random.uniform(1.0, 10.0), 2)
        F = round((m1 * m2) / (r * r), 4)
        writer.writerow([m1, m2, r, F])
print("gravity_data.csv generated.")
