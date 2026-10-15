{ parity probe: x-raise-08 }
unit P_x_raise_08;

function Initialize: integer;
var
  s: string;
begin
  try
    s := Format('%d', [1.5]);
  except
    on E: Exception do AddMessage(E.ClassName + ': ' + E.Message);
  end;
  AddMessage('after');
end;
end.
