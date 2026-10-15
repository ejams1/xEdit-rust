{ parity probe: x-121 }
unit P_x_121;

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
  s := 1.5 + 'x'; AddMessage(s);
  AddMessage('after');
end;
end.
