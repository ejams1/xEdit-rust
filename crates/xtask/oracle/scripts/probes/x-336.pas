{ parity probe: x-336 }
unit P_x_336;

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
  v := TStringList.Create; AddMessage(v.ClassName);
  AddMessage('after');
end;
end.
