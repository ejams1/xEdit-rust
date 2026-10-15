{ parity probe: x-106 }
unit P_x_106;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  AddMessage('a' + #0 + 'b');
  AddMessage('after');
end;
end.
