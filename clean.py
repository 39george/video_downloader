#!/usr/bin/env python3
import sys
import os
import hashlib

RED = "\033[31m"
GREEN = "\033[32m"
RESET = "\033[0m"

def find_all(path: str):
    list_of_files: dict[str, list[str]] = {}
    idx = 0
    for dirpath, _, filenames in os.walk(path):
        idx += 1

        for filename in filenames:
            filepath = os.sep.join([dirpath, filename])
            with open(filepath, 'rb') as file_to_check:
                # read contents of the file
                data = file_to_check.read()
                # pipe contents of the file through
                md5_returned = hashlib.md5(data).hexdigest()

                if md5_returned not in list_of_files:
                  list_of_files[md5_returned] = [] 

                list_of_files[md5_returned].append(filepath)
                print('Hash: ', md5_returned, 'path: ', filepath)

    return list_of_files

def find_duplicates(data: dict[str, list[str]]):
    for key in data:
        if len(data[key]) > 1:
            for path in data[key]:
                if 'hard mode' in path.lower():
                    print(GREEN, 'Found duplication in the Hard Mode directory: ', key, RESET)
                    confirmation = input("Confirm deletion (y/N): ").lower()

                    if confirmation == 'y':
                        # Try to delete the file.
                        try:
                            os.remove(path)
                        except OSError as e:
                            # If it fails, inform the user.
                            print(RED, "Error: %s - %s." % (e.filename, e.strerror), RESET)
                else:
                    print(RED, 'Found duplication not in hard mode directory: ', path, RESET)

def cleanup_directories(path: str):
    for dirpath, _, _ in os.walk(path):
        if not os.listdir(dirpath):
            os.rmdir(dirpath)
            print('Directory ', dirpath, ' is deleted.')

def main():
    path = sys.argv[1]
    find_duplicates(find_all(path))
    cleanup_directories(path)


if __name__ == '__main__':
    main()
