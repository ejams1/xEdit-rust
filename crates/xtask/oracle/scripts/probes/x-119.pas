{ parity probe: x-119 }
unit P_x_119;

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
  s := 5 + 'x'; AddMessage(s);
  AddMessage('after');
end;
end.
