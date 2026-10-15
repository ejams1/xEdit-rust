{ parity probe: x-raise-25 }
unit P_x_raise_25;

function Initialize: integer;
begin
  raise EAbort.Create('silent');
end;
end.
