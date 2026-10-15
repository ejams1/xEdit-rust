{ parity probe: x-raise-02 }
unit P_x_raise_02;

function Initialize: integer;
begin
  raise Exception.CreateFmt('fmt %d', [5]);
end;
end.
