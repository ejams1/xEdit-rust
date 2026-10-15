{ parity probe: x-raise-29 }
unit P_x_raise_29;

function Initialize: integer;
begin
  raise ERangeError.Create('re');
end;
end.
