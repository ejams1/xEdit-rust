{ parity probe: x-raise-06 }
unit P_x_raise_06;

function Initialize: integer;
var
  v: Variant;
begin
  try
    v := 'x' + 5;
  except
    on E: Exception do AddMessage(E.ClassName + ': ' + E.Message);
  end;
  AddMessage('after');
end;
end.
