{ parity probe: x-raise-23 }
unit P_x_raise_23;

function Initialize: integer;
begin
  raise Exception.Create('');
end;
end.
