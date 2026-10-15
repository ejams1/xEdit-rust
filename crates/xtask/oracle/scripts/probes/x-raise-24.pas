{ parity probe: x-raise-24 }
unit P_x_raise_24;

function Initialize: integer;
begin
  raise Exception.Create('line one' + #13#10 + 'line two');
end;
end.
