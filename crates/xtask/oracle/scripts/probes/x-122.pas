{ parity probe: x-122 }
unit P_x_122;

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
  s := True + 'x'; AddMessage(s);
  AddMessage('after');
end;
end.
