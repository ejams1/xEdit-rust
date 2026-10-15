{ parity probe: x-257 }
unit P_x_257;

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
  v := IntToStr(Null); AddMessage(v);
  AddMessage('after');
end;
end.
