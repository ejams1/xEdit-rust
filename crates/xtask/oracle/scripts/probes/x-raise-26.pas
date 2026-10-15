{ parity probe: x-raise-26 }
unit P_x_raise_26;

function Initialize: integer;
begin
  raise EStringListError.Create('sle');
end;
end.
