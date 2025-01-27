import argparse
import glob
import os
import sys

from datetime import datetime


BEGINNING_DATE = "2023_04"


def run_pmex():
    import requests


def generate_addresses_file(data_dir: str) -> str:
    print("run_pmex.py # Generating addresses file", flush=True)
    output_dir = f"{data_dir}/vega"
    borrowers = f"{data_dir}/vega/borrowers"

    # Find all raw files and collect addresses
    addresses = set()
    for filepath in glob.glob(f"{borrowers}/*_raw.txt"):
        with open(filepath, 'r') as f:
            addresses.update(line.strip() for line in f if line.strip())

    # Generate output filename with timestamp and count
    timestamp = datetime.now().strftime("%Y%m%d%H%M%S")
    address_count = len(addresses)
    output_file = f"{output_dir}/addresses_{timestamp}_{address_count}.txt"

    # Write unique addresses to output file
    with open(output_file, 'w') as f:
        for address in sorted(addresses):
            f.write(f"{address}\n")

    print(f"run_pmex.py # Generated file with {address_count} addresses", flush=True)
    return output_file

def main():
    data_dir = os.getenv('DATA_DIR')
    if not data_dir:
        print("run_pmex.py # DATA_DIR environment variable not set or empty", 
              file=sys.stderr, flush=True)
        return 1
    print(f"run_pmex.py # Running with DATA_DIR = {data_dir}", flush=True)

    parser = argparse.ArgumentParser(description='Process PMEX data')
    parser.add_argument('--use-local-data', type=str, default='true',
                       help='Whether to use local data (true/false)')
    
    try:
        args = parser.parse_args()
        run_pmex()
        result = generate_addresses_file(data_dir)
        print(result, flush=True)
        return 0
    except Exception as e:
        print(f"run_pmex.py # Error: {str(e)}", file=sys.stderr, flush=True)
        return 1

if __name__ == "__main__":
    sys.exit(main())
